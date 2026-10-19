use core::ops::RangeBounds;

use crate::{IntegerTooSmall, RoaringBitmap128};

impl RoaringBitmap128 {
    /// Creates an empty `RoaringBitmap128`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    /// let rb = RoaringBitmap128::new();
    /// ```
    pub fn new() -> RoaringBitmap128 {
        RoaringBitmap128 {
            map: alloc::collections::BTreeMap::new(),
            len: 0,
        }
    }

    /// Adds a value to the set. Returns `true` if the value was not already present in the set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert_eq!(rb.insert(3), true);
    /// assert_eq!(rb.insert(3), false);
    /// assert_eq!(rb.contains(3), true);
    /// ```
    pub fn insert(&mut self, value: u128) -> bool {
        todo!("{value}")
    }

    /// Inserts a range of values.
    ///
    /// Returns the number of inserted values.
    ///
    /// Every key (`value >> 16`) touched by the range gets a container, so huge ranges allocate
    /// accordingly (as with `RoaringTreemap`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// rb.insert_range(2..4);
    /// assert!(rb.contains(2));
    /// assert!(rb.contains(3));
    /// assert!(!rb.contains(4));
    /// ```
    pub fn insert_range<R: RangeBounds<u128>>(&mut self, range: R) -> u64 {
        let _ = range;
        todo!()
    }

    /// Pushes `value` in the bitmap only if it is greater than the current maximum value.
    ///
    /// Returns whether the value was inserted.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// # #[allow(deprecated)]
    /// # {
    /// assert!(rb.push(1));
    /// assert!(rb.push(3));
    /// assert_eq!(rb.push(3), false);
    /// assert!(rb.push(5));
    /// # }
    ///
    /// assert_eq!(rb.iter().collect::<Vec<u128>>(), vec![1, 3, 5]);
    /// ```
    #[deprecated(since = "0.1.0", note = "use `try_push` instead")]
    pub fn push(&mut self, value: u128) -> bool {
        self.try_push(value).is_ok()
    }

    /// Pushes `value` in the bitmap only if it is greater than the current maximum value.
    ///
    /// Returns an error if the value is not greater than the current maximum value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::{RoaringBitmap128, IntegerTooSmall};
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert!(rb.try_push(1).is_ok());
    /// assert!(rb.try_push(3).is_ok());
    /// assert_eq!(rb.try_push(3), Err(IntegerTooSmall));
    /// assert!(rb.try_push(5).is_ok());
    ///
    /// assert_eq!(rb.iter().collect::<Vec<u128>>(), vec![1, 3, 5]);
    /// ```
    pub fn try_push(&mut self, value: u128) -> Result<(), IntegerTooSmall> {
        todo!("{value}")
    }

    /// Removes a value from the set. Returns `true` if the value was present in the set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// rb.insert(3);
    /// assert_eq!(rb.remove(3), true);
    /// assert_eq!(rb.remove(3), false);
    /// assert_eq!(rb.contains(3), false);
    /// ```
    pub fn remove(&mut self, value: u128) -> bool {
        todo!("{value}")
    }

    /// Removes a range of values.
    /// Returns the number of removed values.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// rb.insert(2);
    /// rb.insert(3);
    /// assert_eq!(rb.remove_range(2..4), 2);
    /// ```
    pub fn remove_range<R: RangeBounds<u128>>(&mut self, range: R) -> u64 {
        let _ = range;
        todo!()
    }

    /// Returns `true` if this set contains the specified integer.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// rb.insert(1);
    /// assert_eq!(rb.contains(0), false);
    /// assert_eq!(rb.contains(1), true);
    /// assert_eq!(rb.contains(100), false);
    /// ```
    pub fn contains(&self, value: u128) -> bool {
        todo!("{value}")
    }

    /// Returns `true` if all values in the range are present in this set.
    ///
    /// An empty range is always contained.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// // An empty range is always contained
    /// assert!(rb.contains_range(7..7));
    ///
    /// rb.insert_range(1..0x1_0000_0000);
    /// assert!(rb.contains_range(1..0x1_0000_0000));
    /// assert!(rb.contains_range(2..0x1_0000_0000));
    /// // 0 is not contained
    /// assert!(!rb.contains_range(0..2));
    /// // 0x1_0000_0000 is not contained
    /// assert!(!rb.contains_range(1..=0x1_0000_0000));
    /// ```
    pub fn contains_range<R: RangeBounds<u128>>(&self, range: R) -> bool {
        let _ = range;
        todo!()
    }

    /// Returns the number of elements in this set which are in the passed range.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// rb.insert_range(0x1_0000_0000..0x4_0000_0000);
    /// rb.insert(0x5_0000_0001);
    /// rb.insert(0x5_0000_0005);
    /// rb.insert(u128::MAX);
    ///
    /// assert_eq!(rb.range_cardinality(0..0x1_0000_0000), 0);
    /// assert_eq!(rb.range_cardinality(0x1_0000_0000..0x4_0000_0000), 0x3_0000_0000);
    /// assert_eq!(rb.range_cardinality(0x5_0000_0000..0x6_0000_0000), 2);
    /// assert_eq!(rb.range_cardinality(0x1_0000_0000..0x1_0000_0000), 0);
    /// assert_eq!(rb.range_cardinality(0x5_0000_0000..=u128::MAX), 3);
    /// ```
    pub fn range_cardinality<R: RangeBounds<u128>>(&self, range: R) -> u64 {
        let _ = range;
        todo!()
    }

    /// Clears all integers in this set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// rb.insert(1);
    /// assert_eq!(rb.contains(1), true);
    /// rb.clear();
    /// assert_eq!(rb.contains(1), false);
    /// ```
    pub fn clear(&mut self) {
        self.map.clear();
        self.len = 0;
    }

    /// Returns `true` if there are no integers in this set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert_eq!(rb.is_empty(), true);
    ///
    /// rb.insert(3);
    /// assert_eq!(rb.is_empty(), false);
    /// ```
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns `true` if there are every possible integers in this set.
    ///
    /// A full 128-bit set has 2<sup>128</sup> values spread over 2<sup>112</sup> containers and
    /// cannot be materialized, so this always returns `false` in practice. It is provided for API
    /// parity with `RoaringBitmap` / `RoaringTreemap`.
    pub fn is_full(&self) -> bool {
        todo!()
    }

    /// Returns the number of distinct integers added to the set.
    ///
    /// This is `O(1)`: the cardinality is maintained by every operation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert_eq!(rb.len(), 0);
    ///
    /// rb.insert(3);
    /// assert_eq!(rb.len(), 1);
    ///
    /// rb.insert(3);
    /// rb.insert(4);
    /// assert_eq!(rb.len(), 2);
    /// ```
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Returns the minimum value in the set (if the set is non-empty).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert_eq!(rb.min(), None);
    ///
    /// rb.insert(3);
    /// rb.insert(4);
    /// assert_eq!(rb.min(), Some(3));
    /// ```
    pub fn min(&self) -> Option<u128> {
        todo!()
    }

    /// Returns the maximum value in the set (if the set is non-empty).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert_eq!(rb.max(), None);
    ///
    /// rb.insert(3);
    /// rb.insert(4);
    /// assert_eq!(rb.max(), Some(4));
    /// ```
    pub fn max(&self) -> Option<u128> {
        todo!()
    }

    /// Returns the number of integers that are <= value. rank(u128::MAX) == len()
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert_eq!(rb.rank(0), 0);
    ///
    /// rb.insert(3);
    /// rb.insert(4);
    /// assert_eq!(rb.rank(3), 1);
    /// assert_eq!(rb.rank(10), 2)
    /// ```
    pub fn rank(&self, value: u128) -> u64 {
        todo!("{value}")
    }

    /// Returns the `n`th integer in the set or `None` if `n >= len()`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// assert_eq!(rb.select(0), None);
    ///
    /// rb.append(vec![0, 10, 100]).unwrap();
    ///
    /// assert_eq!(rb.select(0), Some(0));
    /// assert_eq!(rb.select(1), Some(10));
    /// assert_eq!(rb.select(2), Some(100));
    /// assert_eq!(rb.select(3), None);
    /// ```
    pub fn select(&self, n: u64) -> Option<u128> {
        todo!("{n}")
    }

    /// Removes the `n` smallests values from this bitmap.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::from_iter([1, 5, 7, 9]);
    /// rb.remove_smallest(2);
    /// assert_eq!(rb, RoaringBitmap128::from_iter([7, 9]));
    ///
    /// let mut rb = RoaringBitmap128::from_iter([1, 3, 7, 9]);
    /// rb.remove_smallest(2);
    /// assert_eq!(rb, RoaringBitmap128::from_iter([7, 9]));
    /// ```
    pub fn remove_smallest(&mut self, n: u64) {
        todo!("{n}")
    }

    /// Removes the `n` biggests values from this bitmap.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::from_iter([1, 5, 7, 9]);
    /// rb.remove_biggest(2);
    /// assert_eq!(rb, RoaringBitmap128::from_iter([1, 5]));
    /// rb.remove_biggest(1);
    /// assert_eq!(rb, RoaringBitmap128::from_iter([1]));
    /// ```
    pub fn remove_biggest(&mut self, n: u64) {
        todo!("{n}")
    }

    /// Optimizes the container storage: each container is converted to the run representation
    /// if that is smaller, and run containers are converted back otherwise.
    ///
    /// Returns `true` if the storage was modified.
    ///
    /// # Examples
    ///
    /// ```
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::from_iter(1000..100000);
    /// rb.optimize();
    /// ```
    pub fn optimize(&mut self) -> bool {
        todo!()
    }

    /// Removes run-length encoding even when it is more space efficient.
    ///
    /// Returns `true` if a change was applied.
    ///
    /// # Examples
    ///
    /// ```
    /// use roaring128::RoaringBitmap128;
    ///
    /// let mut rb = RoaringBitmap128::new();
    /// rb.insert_range(0..=10000);
    /// rb.remove_run_compression();
    /// assert_eq!(rb.len(), 10001);
    /// ```
    pub fn remove_run_compression(&mut self) -> bool {
        todo!()
    }

    /// Creates a bitmap from a little-endian, LSB-0 bit sequence: bit `i` of `bytes` (bit
    /// `i % 8` of `bytes[i / 8]`) sets the value `offset + i`.
    ///
    /// # Panics
    ///
    /// Panics if `offset + bytes.len() * 8` exceeds `2^128`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use roaring128::RoaringBitmap128;
    ///
    /// let bytes = [0b00000101, 0b00000010, 0b00000000, 0b10000000];
    /// //             ^^^^^^^^    ^^^^^^^^    ^^^^^^^^    ^^^^^^^^
    /// //             76543210          98
    /// let rb = RoaringBitmap128::from_lsb0_bytes(0, &bytes);
    /// assert!(rb.contains(0));
    /// assert!(!rb.contains(1));
    /// assert!(rb.contains(2));
    /// assert!(rb.contains(9));
    /// assert!(rb.contains(31));
    ///
    /// let rb = RoaringBitmap128::from_lsb0_bytes(8, &bytes);
    /// assert!(rb.contains(8));
    /// assert!(!rb.contains(9));
    /// assert!(rb.contains(10));
    /// assert!(rb.contains(17));
    /// assert!(rb.contains(39));
    /// ```
    pub fn from_lsb0_bytes(offset: u128, bytes: &[u8]) -> RoaringBitmap128 {
        todo!("{offset} {}", bytes.len())
    }

    /// Checks the internal invariants (sorted keys, valid containers, no empty container and a
    /// correct cached cardinality).
    #[doc(hidden)]
    pub fn internal_validate(&self) -> Result<(), &'static str> {
        todo!()
    }
}

impl Default for RoaringBitmap128 {
    fn default() -> RoaringBitmap128 {
        RoaringBitmap128::new()
    }
}

impl Clone for RoaringBitmap128 {
    fn clone(&self) -> Self {
        RoaringBitmap128 {
            map: self.map.clone(),
            len: self.len,
        }
    }

    fn clone_from(&mut self, other: &Self) {
        self.map.clone_from(&other.map);
        self.len = other.len;
    }
}

impl PartialEq for RoaringBitmap128 {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.map == other.map
    }
}

impl Eq for RoaringBitmap128 {}
