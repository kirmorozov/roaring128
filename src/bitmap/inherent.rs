use alloc::collections::BTreeMap;
use alloc::collections::btree_map::Entry;
use alloc::vec::Vec;
use core::mem;
use core::ops::{Bound, RangeBounds, RangeInclusive};

use crate::container::{BITMAP_BYTES, Container, Kind};
use crate::util::{MAX_KEY, container_range, convert_range_to_inclusive, join, split, split_range};
use crate::{IntegerTooSmall, RoaringBitmap128};

/// Number of values in a full container.
const FULL_LEN: u64 = 1 << 16;

/// Number of keys in `lo..=hi` (`lo <= hi`), saturated to `usize`.
#[inline]
fn key_span(lo: u128, hi: u128) -> u128 {
    hi - lo + 1
}

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
        let (key, index) = split(value);
        let inserted = match self.map.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(Container::with_single(index));
                true
            }
            Entry::Occupied(mut entry) => entry.get_mut().insert(index),
        };
        self.len += inserted as u64;
        inserted
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
        let Ok(range) = convert_range_to_inclusive(range) else {
            return 0;
        };
        let (start_key, start_index, end_key, end_index) = split_range(&range);

        let inserted = if start_key == end_key {
            self.insert_range_in(start_key, start_index..=end_index)
        } else {
            let mut inserted = self.insert_range_in(start_key, start_index..=u16::MAX);
            if end_key - start_key > 1 {
                inserted += self.fill_keys(start_key + 1, end_key - 1);
            }
            inserted + self.insert_range_in(end_key, 0..=end_index)
        };
        self.len += inserted;
        inserted
    }

    /// Inserts `range` into the container `key` (creating it if needed). Does not touch `len`.
    fn insert_range_in(&mut self, key: u128, range: RangeInclusive<u16>) -> u64 {
        match self.map.entry(key) {
            Entry::Vacant(entry) => {
                let inserted = u64::from(*range.end() - *range.start()) + 1;
                entry.insert(Container::from_range(range));
                inserted
            }
            Entry::Occupied(mut entry) => entry.get_mut().insert_range(range),
        }
    }

    /// Makes every container of `lo..=hi` (`lo <= hi`) full. Returns the number of inserted
    /// values; does not touch `len`.
    ///
    /// Small spans use one B-tree entry per key. Spans that are large compared to the index
    /// rebuild it in a single linear pass and bulk-build the result instead.
    fn fill_keys(&mut self, lo: u128, hi: u128) -> u64 {
        let span = key_span(lo, hi);
        let total = span * FULL_LEN as u128;
        let small = span < 32 || span.saturating_mul(4) < self.map.len() as u128;
        if small {
            let mut inserted = 0;
            for key in lo..=hi {
                match self.map.entry(key) {
                    Entry::Vacant(entry) => {
                        entry.insert(Container::full());
                        inserted += FULL_LEN;
                    }
                    Entry::Occupied(mut entry) => {
                        // Like `roaring`, a filled container ends up as a single run, even
                        // one that was already full as a bitmap.
                        let container = entry.get_mut();
                        if !(container.kind() == Kind::Run && container.is_full()) {
                            inserted += FULL_LEN - container.len();
                            *container = Container::full();
                        }
                    }
                }
            }
            return inserted;
        }

        let span = usize::try_from(span).expect("range too large to be materialized");
        let old = mem::take(&mut self.map);
        let mut entries: Vec<(u128, Container)> = Vec::with_capacity(old.len() + span);
        let mut existing = 0u128;
        let mut filled = false;
        for (key, container) in old {
            if key < lo {
                entries.push((key, container));
            } else if key <= hi {
                existing += container.len() as u128;
            } else {
                if !filled {
                    entries.extend((lo..=hi).map(|k| (k, Container::full())));
                    filled = true;
                }
                entries.push((key, container));
            }
        }
        if !filled {
            entries.extend((lo..=hi).map(|k| (k, Container::full())));
        }
        self.map = BTreeMap::from_iter(entries);
        u64::try_from(total - existing).expect("cardinality overflow")
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
        let (key, index) = split(value);
        match self.map.last_entry() {
            Some(mut entry) if *entry.key() == key => {
                if entry.get_mut().push(index) {
                    self.len += 1;
                    Ok(())
                } else {
                    Err(IntegerTooSmall)
                }
            }
            Some(entry) if *entry.key() > key => Err(IntegerTooSmall),
            _ => {
                self.map.insert(key, Container::with_single(index));
                self.len += 1;
                Ok(())
            }
        }
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
        let (key, index) = split(value);
        let Entry::Occupied(mut entry) = self.map.entry(key) else {
            return false;
        };
        if !entry.get_mut().remove(index) {
            return false;
        }
        if entry.get().is_empty() {
            entry.remove();
        }
        self.len -= 1;
        true
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
        let Ok(range) = convert_range_to_inclusive(range) else {
            return 0;
        };
        let (start_key, start_index, end_key, end_index) = split_range(&range);

        let mut removed = 0;
        // One pass over the touched containers: fully covered containers are dropped, partially
        // covered ones are trimmed and dropped once empty.
        self.map
            .extract_if(start_key..=end_key, |&key, container| {
                let range = container_range(key, start_key, start_index, end_key, end_index);
                if range == (0..=u16::MAX) {
                    removed += container.len();
                    true
                } else {
                    removed += container.remove_range(range);
                    container.is_empty()
                }
            })
            .for_each(drop);
        self.len -= removed;
        removed
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
        let (key, index) = split(value);
        self.map.get(&key).is_some_and(|c| c.contains(index))
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
        let Ok(range) = convert_range_to_inclusive(range) else {
            // Empty ranges are always contained.
            return true;
        };
        let (start_key, start_index, end_key, end_index) = split_range(&range);
        if start_key == end_key {
            return self
                .map
                .get(&start_key)
                .is_some_and(|c| c.contains_range(start_index..=end_index));
        }
        // Every key of the span needs a container.
        if (self.map.len() as u128) < key_span(start_key, end_key) {
            return false;
        }
        let mut expected = start_key;
        for (&key, container) in self.map.range(start_key..=end_key) {
            if key != expected {
                return false;
            }
            let range = container_range(key, start_key, start_index, end_key, end_index);
            let contained = if range == (0..=u16::MAX) {
                container.is_full()
            } else {
                container.contains_range(range)
            };
            if !contained {
                return false;
            }
            // `key <= MAX_KEY < u128::MAX`, no overflow.
            expected = key + 1;
        }
        expected == end_key + 1
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
        let Ok(range) = convert_range_to_inclusive(range) else {
            return 0;
        };
        let (start_key, start_index, end_key, end_index) = split_range(&range);
        if start_key == end_key {
            return self
                .map
                .get(&start_key)
                .map_or(0, |c| c.range_cardinality(start_index..=end_index));
        }
        let mut edges_full = 0;
        let mut edges = 0;
        if let Some(c) = self.map.get(&start_key) {
            edges_full += c.len();
            edges += if start_index == 0 {
                c.len()
            } else {
                c.range_cardinality(start_index..=u16::MAX)
            };
        }
        if let Some(c) = self.map.get(&end_key) {
            edges_full += c.len();
            edges += if end_index == u16::MAX {
                c.len()
            } else {
                c.range_cardinality(0..=end_index)
            };
        }
        edges + self.len_strictly_between(start_key, end_key, edges_full)
    }

    /// Sum of the cardinalities of the containers whose keys are strictly between `lo` and `hi`
    /// (`lo < hi`); `edges` is the cardinality of the containers at `lo` and `hi`.
    ///
    /// Walks the inside and the outside of the key span in lockstep and stops at the first
    /// exhausted side, so the cost is `O(log n + min(inside, outside))`: the outside sum gives the
    /// answer through the cached total cardinality.
    pub(crate) fn len_strictly_between(&self, lo: u128, hi: u128, edges: u64) -> u64 {
        debug_assert!(lo < hi);
        let mut inside = self.map.range((Bound::Excluded(lo), Bound::Excluded(hi)));
        let mut outside = self
            .map
            .range(..lo)
            .chain(self.map.range((Bound::Excluded(hi), Bound::Unbounded)));
        let mut sum_inside = 0;
        let mut sum_outside = 0;
        loop {
            match inside.next() {
                Some((_, c)) => sum_inside += c.len(),
                None => return sum_inside,
            }
            match outside.next() {
                Some((_, c)) => sum_outside += c.len(),
                None => return self.len - sum_outside - edges,
            }
        }
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
        // A full set needs MAX_KEY + 1 = 2^112 full containers; an index that large cannot exist
        // in memory, but keep the exact definition.
        self.map.len() as u128 > MAX_KEY && self.map.values().all(Container::is_full)
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
        let (&key, container) = self.map.first_key_value()?;
        container.min().map(|index| join(key, index))
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
        let (&key, container) = self.map.last_key_value()?;
        container.max().map(|index| join(key, index))
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
        let (key, index) = split(value);
        match (self.map.first_key_value(), self.map.last_key_value()) {
            (Some((&first, _)), _) if key < first => return 0,
            (_, Some((&last, _))) if key > last => return self.len,
            (None, _) | (_, None) => return 0,
            _ => {}
        }
        let at = self.map.get(&key);
        let at_rank = at.map_or(0, |c| c.rank(index));
        let at_len = at.map_or(0, Container::len);

        // Sum the shorter side: walk both sides of `key` in lockstep, the first exhausted one
        // gives the answer (directly or through the cached total cardinality).
        let mut before = self.map.range(..key);
        let mut after = self.map.range((Bound::Excluded(key), Bound::Unbounded));
        let mut sum_before = 0;
        let mut sum_after = 0;
        loop {
            match before.next() {
                Some((_, c)) => sum_before += c.len(),
                None => return sum_before + at_rank,
            }
            match after.next() {
                Some((_, c)) => sum_after += c.len(),
                None => return self.len - sum_after - at_len + at_rank,
            }
        }
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
        if n >= self.len {
            return None;
        }
        if n < self.len / 2 {
            let mut n = n;
            for (&key, container) in &self.map {
                let len = container.len();
                if n < len {
                    return container.select(n as u16).map(|index| join(key, index));
                }
                n -= len;
            }
        } else {
            // Position counted from the back.
            let mut m = self.len - 1 - n;
            for (&key, container) in self.map.iter().rev() {
                let len = container.len();
                if m < len {
                    return container
                        .select((len - 1 - m) as u16)
                        .map(|index| join(key, index));
                }
                m -= len;
            }
        }
        debug_assert!(false, "cached cardinality is out of sync");
        None
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
        if n == 0 {
            return;
        }
        if n >= self.len {
            self.clear();
            return;
        }
        // Find the first container that survives (partially or entirely).
        let mut remaining = n;
        let mut boundary = None;
        for (&key, container) in &self.map {
            let len = container.len();
            if len > remaining {
                boundary = Some(key);
                break;
            }
            remaining -= len;
        }
        let boundary = boundary.expect("n < len");
        if self
            .map
            .first_key_value()
            .is_some_and(|(&k, _)| k != boundary)
        {
            // Drops every container below `boundary` with one O(log n) split.
            self.map = self.map.split_off(&boundary);
        }
        if remaining > 0 {
            let mut first = self.map.first_entry().expect("non empty");
            first.get_mut().remove_smallest(remaining);
        }
        self.len -= n;
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
        if n == 0 {
            return;
        }
        if n >= self.len {
            self.clear();
            return;
        }
        // Find the last container that survives (partially or entirely).
        let mut remaining = n;
        let mut boundary = None;
        for (&key, container) in self.map.iter().rev() {
            let len = container.len();
            if len > remaining {
                boundary = Some(key);
                break;
            }
            remaining -= len;
        }
        let boundary = boundary.expect("n < len");
        if self
            .map
            .last_key_value()
            .is_some_and(|(&k, _)| k != boundary)
        {
            // `boundary <= MAX_KEY < u128::MAX`: drops every container above it with one split.
            drop(self.map.split_off(&(boundary + 1)));
        }
        if remaining > 0 {
            let mut last = self.map.last_entry().expect("non empty");
            last.get_mut().remove_biggest(remaining);
        }
        self.len -= n;
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
        let mut changed = false;
        for container in self.map.values_mut() {
            changed |= container.optimize();
        }
        changed
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
        let mut changed = false;
        for container in self.map.values_mut() {
            changed |= container.remove_run_compression();
        }
        changed
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
        if bytes.is_empty() {
            return RoaringBitmap128::new();
        }
        let bits = bytes.len() as u128 * 8;
        assert!(
            offset.checked_add(bits - 1).is_some(),
            "offset + bytes.len() * 8 must be <= 2^128"
        );

        // A non byte-aligned offset is handled by shifting the input on the fly, one container
        // chunk at a time: virtual byte `j` describes the values `base + 8 * j ..+ 8`.
        let shift = (offset % 8) as u32;
        let base = offset - shift as u128;
        let virtual_len = bytes.len() + (shift != 0) as usize;
        let mut shifted = if shift != 0 {
            alloc::vec![0u8; BITMAP_BYTES.min(virtual_len)]
        } else {
            Vec::new()
        };

        let mut entries = Vec::with_capacity(virtual_len / BITMAP_BYTES + 2);
        let mut len = 0;
        let mut j = 0;
        while j < virtual_len {
            // `base + 8 * j < 2^128` because the last value `offset + bits - 1` fits.
            let (key, index) = split(base + 8 * j as u128);
            let byte_offset = index as usize / 8;
            let take = (BITMAP_BYTES - byte_offset).min(virtual_len - j);
            let chunk = if shift == 0 {
                &bytes[j..j + take]
            } else {
                for (t, out) in shifted[..take].iter_mut().enumerate() {
                    let v = j + t;
                    let low = bytes.get(v).map_or(0, |&b| b << shift);
                    let high = v
                        .checked_sub(1)
                        .and_then(|p| bytes.get(p))
                        .map_or(0, |&b| b >> (8 - shift));
                    *out = low | high;
                }
                &shifted[..take]
            };
            if let Some(container) = Container::from_lsb0_bytes(chunk, byte_offset) {
                len += container.len();
                entries.push((key, container));
            }
            j += take;
        }
        RoaringBitmap128::from_sorted_entries_with_len(entries, len)
    }

    /// Checks the internal invariants (sorted keys, valid containers, no empty container and a
    /// correct cached cardinality).
    #[doc(hidden)]
    pub fn internal_validate(&self) -> Result<(), &'static str> {
        let mut len = 0u64;
        let mut previous = None;
        for (&key, container) in &self.map {
            if key > MAX_KEY {
                return Err("container key is out of range");
            }
            if previous.is_some_and(|p| p >= key) {
                return Err("container keys are not strictly increasing");
            }
            previous = Some(key);
            if container.is_empty() {
                return Err("empty container in the index");
            }
            container.internal_validate()?;
            len = len
                .checked_add(container.len())
                .ok_or("cardinality overflows u64")?;
        }
        if len != self.len {
            return Err("cached cardinality does not match the containers");
        }
        Ok(())
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
