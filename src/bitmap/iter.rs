use alloc::collections::BTreeMap;
use alloc::collections::btree_map;
use alloc::vec::Vec;
use core::iter::FusedIterator;
use core::mem;
use core::ops::{RangeBounds, RangeInclusive};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::container::{self, Container};
use crate::util::{ConvertRangeError, MAX_KEY, convert_range_to_inclusive, join, split};
use crate::{NonSortedIntegers, RoaringBitmap128};

/// An iterator for `RoaringBitmap128`.
#[derive(Clone)]
pub struct Iter<'a> {
    inner: Core<RefMiddle<'a>>,
}

/// An iterator for `RoaringBitmap128`.
#[derive(Clone)]
pub struct IntoIter {
    inner: Core<OwnedMiddle>,
}

// ---------------------------------------------------------------------------------------------
// Shared iterator machinery
// ---------------------------------------------------------------------------------------------

/// The operations of the borrowing and owning container iterators used here.
trait ContainerIter: DoubleEndedIterator<Item = u16> + ExactSizeIterator + Clone {
    fn advance_to(&mut self, n: u16);
    fn advance_back_to(&mut self, n: u16);
    fn next_range(&mut self) -> Option<RangeInclusive<u16>>;
    fn next_range_back(&mut self) -> Option<RangeInclusive<u16>>;
    fn peek(&self) -> Option<u16>;
    fn peek_back(&self) -> Option<u16>;
}

macro_rules! impl_container_iter {
    ($ty:ty) => {
        impl ContainerIter for $ty {
            #[inline]
            fn advance_to(&mut self, n: u16) {
                <$ty>::advance_to(self, n)
            }
            #[inline]
            fn advance_back_to(&mut self, n: u16) {
                <$ty>::advance_back_to(self, n)
            }
            #[inline]
            fn next_range(&mut self) -> Option<RangeInclusive<u16>> {
                <$ty>::next_range(self)
            }
            #[inline]
            fn next_range_back(&mut self) -> Option<RangeInclusive<u16>> {
                <$ty>::next_range_back(self)
            }
            #[inline]
            fn peek(&self) -> Option<u16> {
                <$ty>::peek(self)
            }
            #[inline]
            fn peek_back(&self) -> Option<u16> {
                <$ty>::peek_back(self)
            }
        }
    };
}

impl_container_iter!(container::Iter<'_>);
impl_container_iter!(container::IntoIter);

/// The containers of an iterator that have not been started yet, in key order.
trait Middle: Clone {
    /// A container as handed out by the middle (borrowed or owned).
    type Item;
    /// The iterator over one container.
    type Iter: ContainerIter;

    fn next(&mut self) -> Option<(u128, Self::Item)>;
    fn next_back(&mut self) -> Option<(u128, Self::Item)>;
    fn first_key(&self) -> Option<u128>;
    fn last_key(&self) -> Option<u128>;
    /// Drops every container with a key `< key`. The caller guarantees that the first key is
    /// smaller than `key`. Returns the cardinality of the dropped containers when it is known
    /// for free.
    fn skip_below(&mut self, key: u128) -> Option<u64>;
    /// Drops every container with a key `> key`. The caller guarantees that the last key is
    /// greater than `key`.
    fn skip_above(&mut self, key: u128) -> Option<u64>;
    fn clear(&mut self);
    /// Sum of the cardinalities of the remaining containers (linear).
    fn total_len(&self) -> u64;
    fn item_len(item: &Self::Item) -> u64;
    fn item_iter(item: Self::Item) -> Self::Iter;
}

/// The middle of a borrowing iterator: a B-tree range, re-seeked in `O(log n)`.
#[derive(Clone)]
struct RefMiddle<'a> {
    map: &'a BTreeMap<u128, Container>,
    range: btree_map::Range<'a, u128, Container>,
}

impl<'a> Middle for RefMiddle<'a> {
    type Item = &'a Container;
    type Iter = container::Iter<'a>;

    #[inline]
    fn next(&mut self) -> Option<(u128, &'a Container)> {
        self.range.next().map(|(&k, c)| (k, c))
    }

    #[inline]
    fn next_back(&mut self) -> Option<(u128, &'a Container)> {
        self.range.next_back().map(|(&k, c)| (k, c))
    }

    #[inline]
    fn first_key(&self) -> Option<u128> {
        self.range.clone().next().map(|(&k, _)| k)
    }

    #[inline]
    fn last_key(&self) -> Option<u128> {
        self.range.clone().next_back().map(|(&k, _)| k)
    }

    fn skip_below(&mut self, key: u128) -> Option<u64> {
        // Every consumed key is below the first remaining key, which is below `key`, so the new
        // range cannot resurrect consumed containers.
        self.range = match self.last_key() {
            Some(last) if last >= key => self.map.range(key..=last),
            _ => btree_map::Range::default(),
        };
        None
    }

    fn skip_above(&mut self, key: u128) -> Option<u64> {
        self.range = match self.first_key() {
            Some(first) if first <= key => self.map.range(first..=key),
            _ => btree_map::Range::default(),
        };
        None
    }

    fn clear(&mut self) {
        self.range = btree_map::Range::default();
    }

    fn total_len(&self) -> u64 {
        self.range.clone().map(|(_, c)| c.len()).sum()
    }

    #[inline]
    fn item_len(item: &&'a Container) -> u64 {
        item.len()
    }

    #[inline]
    fn item_iter(item: &'a Container) -> container::Iter<'a> {
        item.iter()
    }
}

/// The middle of an owning iterator.
///
/// `btree_map::IntoIter` can neither be cloned nor seeked, so the remaining containers are kept
/// in a vector iterator: sequential iteration stays as cheap as the B-tree's own `IntoIter`,
/// seeks are binary searches (plus dropping the skipped containers, which is unavoidable).
#[derive(Clone)]
struct OwnedMiddle {
    iter: alloc::vec::IntoIter<(u128, Container)>,
}

impl Middle for OwnedMiddle {
    type Item = Container;
    type Iter = container::IntoIter;

    #[inline]
    fn next(&mut self) -> Option<(u128, Container)> {
        self.iter.next()
    }

    #[inline]
    fn next_back(&mut self) -> Option<(u128, Container)> {
        self.iter.next_back()
    }

    #[inline]
    fn first_key(&self) -> Option<u128> {
        self.iter.as_slice().first().map(|(k, _)| *k)
    }

    #[inline]
    fn last_key(&self) -> Option<u128> {
        self.iter.as_slice().last().map(|(k, _)| *k)
    }

    fn skip_below(&mut self, key: u128) -> Option<u64> {
        let slice = self.iter.as_slice();
        let n = slice.partition_point(|(k, _)| *k < key);
        let skipped = slice[..n].iter().map(|(_, c)| c.len()).sum();
        if n > 0 {
            self.iter.nth(n - 1);
        }
        Some(skipped)
    }

    fn skip_above(&mut self, key: u128) -> Option<u64> {
        let slice = self.iter.as_slice();
        let keep = slice.partition_point(|(k, _)| *k <= key);
        let n = slice.len() - keep;
        let skipped = slice[keep..].iter().map(|(_, c)| c.len()).sum();
        if n > 0 {
            self.iter.nth_back(n - 1);
        }
        Some(skipped)
    }

    fn clear(&mut self) {
        self.iter = Vec::new().into_iter();
    }

    fn total_len(&self) -> u64 {
        self.iter.as_slice().iter().map(|(_, c)| c.len()).sum()
    }

    #[inline]
    fn item_len(item: &Container) -> u64 {
        item.len()
    }

    #[inline]
    fn item_iter(item: Container) -> container::IntoIter {
        item.into_iter()
    }
}

/// Sentinel of [`Core::middle_len`]: the cardinality of the middle is not known.
const UNKNOWN: u64 = u64::MAX;

/// A double ended iterator over the values of a sequence of containers.
///
/// Invariant: `keys(front) < keys(middle) < keys(back)`. `front` and `back` are the containers
/// currently being consumed from either end (possibly exhausted), `middle` holds the untouched
/// ones.
struct Core<M: Middle> {
    front: Option<(u128, M::Iter)>,
    middle: M,
    back: Option<(u128, M::Iter)>,
    /// Sum of the cardinalities of the containers in `middle`, or [`UNKNOWN`] after a seek of
    /// a borrowing iterator. Recomputed lazily (and cached) by `size_hint`, so seeks stay
    /// `O(log n)`. An atomic keeps the iterators `Sync`; only relaxed accesses are used.
    middle_len: AtomicU64,
}

impl<M: Middle> Clone for Core<M> {
    fn clone(&self) -> Self {
        Core {
            front: self.front.clone(),
            middle: self.middle.clone(),
            back: self.back.clone(),
            middle_len: AtomicU64::new(self.middle_len.load(Ordering::Relaxed)),
        }
    }
}

#[inline]
fn join_range(key: u128, range: RangeInclusive<u16>) -> RangeInclusive<u128> {
    join(key, *range.start())..=join(key, *range.end())
}

impl<M: Middle> Core<M> {
    fn new(middle: M, middle_len: u64) -> Self {
        Core {
            front: None,
            middle,
            back: None,
            middle_len: AtomicU64::new(middle_len),
        }
    }

    /// Accounts for a container leaving the middle.
    #[inline]
    fn took(&mut self, item: &M::Item) {
        let known = self.middle_len.get_mut();
        if *known != UNKNOWN {
            *known -= M::item_len(item);
        }
    }

    /// Accounts for containers dropped from the middle by a seek.
    #[inline]
    fn skipped(&mut self, skipped: Option<u64>) {
        let known = self.middle_len.get_mut();
        match skipped {
            Some(skipped) if *known != UNKNOWN => *known -= skipped,
            _ => *known = UNKNOWN,
        }
    }

    fn middle_len(&self) -> u64 {
        match self.middle_len.load(Ordering::Relaxed) {
            UNKNOWN => {
                let len = self.middle.total_len();
                self.middle_len.store(len, Ordering::Relaxed);
                len
            }
            len => len,
        }
    }

    /// Moves the first middle container to the front. Returns `false` if the middle is empty.
    #[inline]
    fn pull_front(&mut self) -> bool {
        match self.middle.next() {
            Some((key, item)) => {
                self.took(&item);
                self.front = Some((key, M::item_iter(item)));
                true
            }
            None => false,
        }
    }

    /// Moves the last middle container to the back. Returns `false` if the middle is empty.
    #[inline]
    fn pull_back(&mut self) -> bool {
        match self.middle.next_back() {
            Some((key, item)) => {
                self.took(&item);
                self.back = Some((key, M::item_iter(item)));
                true
            }
            None => false,
        }
    }

    fn remaining(&self) -> u64 {
        let front = self.front.as_ref().map_or(0, |(_, it)| it.len() as u64);
        let back = self.back.as_ref().map_or(0, |(_, it)| it.len() as u64);
        front + self.middle_len() + back
    }

    #[inline]
    fn next(&mut self) -> Option<u128> {
        loop {
            if let Some((key, it)) = &mut self.front {
                if let Some(index) = it.next() {
                    return Some(join(*key, index));
                }
                self.front = None;
            }
            if !self.pull_front() {
                let (key, it) = self.back.as_mut()?;
                return it.next().map(|index| join(*key, index));
            }
        }
    }

    #[inline]
    fn next_back(&mut self) -> Option<u128> {
        loop {
            if let Some((key, it)) = &mut self.back {
                if let Some(index) = it.next_back() {
                    return Some(join(*key, index));
                }
                self.back = None;
            }
            if !self.pull_back() {
                let (key, it) = self.front.as_mut()?;
                return it.next_back().map(|index| join(*key, index));
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match usize::try_from(self.remaining()) {
            Ok(n) => (n, Some(n)),
            Err(_) => (usize::MAX, None),
        }
    }

    fn nth(&mut self, mut n: usize) -> Option<u128> {
        if let Some((key, it)) = &mut self.front {
            let len = it.len();
            if n < len {
                return it.nth(n).map(|index| join(*key, index));
            }
            n -= len;
            self.front = None;
        }
        let known = *self.middle_len.get_mut();
        if known != UNKNOWN && n as u64 >= known {
            // Skip the whole middle without touching its containers.
            n -= known as usize;
            self.middle.clear();
            *self.middle_len.get_mut() = 0;
        } else {
            while let Some((key, item)) = self.middle.next() {
                let len = M::item_len(&item);
                let known = self.middle_len.get_mut();
                if *known != UNKNOWN {
                    *known -= len;
                }
                if (n as u64) < len {
                    let mut it = M::item_iter(item);
                    let value = it.nth(n).map(|index| join(key, index));
                    self.front = Some((key, it));
                    return value;
                }
                n -= len as usize;
            }
        }
        let (key, it) = self.back.as_mut()?;
        it.nth(n).map(|index| join(*key, index))
    }

    fn nth_back(&mut self, mut n: usize) -> Option<u128> {
        if let Some((key, it)) = &mut self.back {
            let len = it.len();
            if n < len {
                return it.nth_back(n).map(|index| join(*key, index));
            }
            n -= len;
            self.back = None;
        }
        let known = *self.middle_len.get_mut();
        if known != UNKNOWN && n as u64 >= known {
            n -= known as usize;
            self.middle.clear();
            *self.middle_len.get_mut() = 0;
        } else {
            while let Some((key, item)) = self.middle.next_back() {
                let len = M::item_len(&item);
                let known = self.middle_len.get_mut();
                if *known != UNKNOWN {
                    *known -= len;
                }
                if (n as u64) < len {
                    let mut it = M::item_iter(item);
                    let value = it.nth_back(n).map(|index| join(key, index));
                    self.back = Some((key, it));
                    return value;
                }
                n -= len as usize;
            }
        }
        let (key, it) = self.front.as_mut()?;
        it.nth_back(n).map(|index| join(*key, index))
    }

    fn fold<B, F: FnMut(B, u128) -> B>(self, init: B, mut f: F) -> B {
        let Core {
            front,
            mut middle,
            back,
            ..
        } = self;
        let mut acc = init;
        if let Some((key, it)) = front {
            acc = it.fold(acc, |acc, index| f(acc, join(key, index)));
        }
        while let Some((key, item)) = middle.next() {
            acc = M::item_iter(item).fold(acc, |acc, index| f(acc, join(key, index)));
        }
        if let Some((key, it)) = back {
            acc = it.fold(acc, |acc, index| f(acc, join(key, index)));
        }
        acc
    }

    fn rfold<B, F: FnMut(B, u128) -> B>(self, init: B, mut f: F) -> B {
        let Core {
            front,
            mut middle,
            back,
            ..
        } = self;
        let mut acc = init;
        if let Some((key, it)) = back {
            acc = it.rfold(acc, |acc, index| f(acc, join(key, index)));
        }
        while let Some((key, item)) = middle.next_back() {
            acc = M::item_iter(item).rfold(acc, |acc, index| f(acc, join(key, index)));
        }
        if let Some((key, it)) = front {
            acc = it.rfold(acc, |acc, index| f(acc, join(key, index)));
        }
        acc
    }

    fn advance_to(&mut self, n: u128) {
        let (key, index) = split(n);
        if let Some((front_key, it)) = &mut self.front {
            match key.cmp(front_key) {
                core::cmp::Ordering::Less => return,
                core::cmp::Ordering::Equal => {
                    it.advance_to(index);
                    return;
                }
                core::cmp::Ordering::Greater => self.front = None,
            }
        }
        if let Some(first) = self.middle.first_key() {
            if first < key {
                let skipped = self.middle.skip_below(key);
                self.skipped(skipped);
            }
            if let Some(first) = self.middle.first_key() {
                if first == key {
                    self.pull_front();
                    if let Some((_, it)) = &mut self.front {
                        it.advance_to(index);
                    }
                }
                // Containers remain in the middle, the back is not affected.
                return;
            }
        }
        if let Some((back_key, it)) = &mut self.back {
            match key.cmp(back_key) {
                core::cmp::Ordering::Less => {}
                core::cmp::Ordering::Equal => it.advance_to(index),
                core::cmp::Ordering::Greater => self.back = None,
            }
        }
    }

    fn advance_back_to(&mut self, n: u128) {
        let (key, index) = split(n);
        if let Some((back_key, it)) = &mut self.back {
            match key.cmp(back_key) {
                core::cmp::Ordering::Greater => return,
                core::cmp::Ordering::Equal => {
                    it.advance_back_to(index);
                    return;
                }
                core::cmp::Ordering::Less => self.back = None,
            }
        }
        if let Some(last) = self.middle.last_key() {
            if last > key {
                let skipped = self.middle.skip_above(key);
                self.skipped(skipped);
            }
            if let Some(last) = self.middle.last_key() {
                if last == key {
                    self.pull_back();
                    if let Some((_, it)) = &mut self.back {
                        it.advance_back_to(index);
                    }
                }
                // Containers remain in the middle, the front is not affected.
                return;
            }
        }
        if let Some((front_key, it)) = &mut self.front {
            match key.cmp(front_key) {
                core::cmp::Ordering::Greater => {}
                core::cmp::Ordering::Equal => it.advance_back_to(index),
                core::cmp::Ordering::Less => self.front = None,
            }
        }
    }

    fn next_range(&mut self) -> Option<RangeInclusive<u128>> {
        let (key, range) = loop {
            if let Some((key, it)) = &mut self.front {
                if let Some(range) = it.next_range() {
                    break (*key, range);
                }
                self.front = None;
            }
            if !self.pull_front() {
                let (key, it) = self.back.as_mut()?;
                return it.next_range().map(|range| join_range(*key, range));
            }
        };
        let start = join(key, *range.start());
        let mut end = join(key, *range.end());
        // A run reaching the end of its container continues in the next key if that container
        // starts at index 0. The current front container is exhausted at that point, so the
        // next container simply becomes the front.
        while end as u16 == u16::MAX {
            let Some(after) = end.checked_add(1) else {
                break;
            };
            let (next_key, _) = split(after);
            if let Some(first) = self.middle.first_key() {
                if first == next_key {
                    self.pull_front();
                    if let Some((_, it)) = &mut self.front
                        && it.peek() == Some(0)
                    {
                        let next = it.next_range().expect("peeked a value");
                        end = join(next_key, *next.end());
                        continue;
                    }
                }
                break;
            }
            if let Some((back_key, it)) = &mut self.back
                && *back_key == next_key
                && it.peek() == Some(0)
            {
                let next = it.next_range().expect("peeked a value");
                end = join(next_key, *next.end());
            }
            break;
        }
        Some(start..=end)
    }

    fn next_range_back(&mut self) -> Option<RangeInclusive<u128>> {
        let (key, range) = loop {
            if let Some((key, it)) = &mut self.back {
                if let Some(range) = it.next_range_back() {
                    break (*key, range);
                }
                self.back = None;
            }
            if !self.pull_back() {
                let (key, it) = self.front.as_mut()?;
                return it.next_range_back().map(|range| join_range(*key, range));
            }
        };
        let mut start = join(key, *range.start());
        let end = join(key, *range.end());
        while start as u16 == 0 {
            let Some(before) = start.checked_sub(1) else {
                break;
            };
            let (prev_key, _) = split(before);
            if let Some(last) = self.middle.last_key() {
                if last == prev_key {
                    self.pull_back();
                    if let Some((_, it)) = &mut self.back
                        && it.peek_back() == Some(u16::MAX)
                    {
                        let prev = it.next_range_back().expect("peeked a value");
                        start = join(prev_key, *prev.start());
                        continue;
                    }
                }
                break;
            }
            if let Some((front_key, it)) = &mut self.front
                && *front_key == prev_key
                && it.peek_back() == Some(u16::MAX)
            {
                let prev = it.next_range_back().expect("peeked a value");
                start = join(prev_key, *prev.start());
            }
            break;
        }
        Some(start..=end)
    }
}

impl<'a> Iter<'a> {
    fn new(rb: &'a RoaringBitmap128) -> Iter<'a> {
        let middle = RefMiddle {
            map: &rb.map,
            range: rb.map.range(..),
        };
        Iter {
            inner: Core::new(middle, rb.len),
        }
    }
}

impl IntoIter {
    fn from_entries(entries: Vec<(u128, Container)>, len: u64) -> IntoIter {
        IntoIter {
            inner: Core::new(
                OwnedMiddle {
                    iter: entries.into_iter(),
                },
                len,
            ),
        }
    }

    fn empty() -> IntoIter {
        IntoIter::from_entries(Vec::new(), 0)
    }
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
                self.inner.advance_to(n)
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
                self.inner.advance_back_to(n)
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
                self.inner.next_range()
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
                self.inner.next_range_back()
            }
        }

        impl Iterator for $ty {
            type Item = u128;

            #[inline]
            fn next(&mut self) -> Option<u128> {
                self.inner.next()
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                self.inner.size_hint()
            }

            fn count(self) -> usize {
                self.inner.remaining() as usize
            }

            fn nth(&mut self, n: usize) -> Option<u128> {
                self.inner.nth(n)
            }

            fn last(mut self) -> Option<u128> {
                self.inner.next_back()
            }

            fn min(mut self) -> Option<u128> {
                self.inner.next()
            }

            fn max(mut self) -> Option<u128> {
                self.inner.next_back()
            }

            #[inline]
            fn fold<B, F>(self, init: B, f: F) -> B
            where
                F: FnMut(B, u128) -> B,
            {
                self.inner.fold(init, f)
            }
        }

        impl DoubleEndedIterator for $ty {
            #[inline]
            fn next_back(&mut self) -> Option<u128> {
                self.inner.next_back()
            }

            fn nth_back(&mut self, n: usize) -> Option<u128> {
                self.inner.nth_back(n)
            }

            #[inline]
            fn rfold<B, F>(self, init: B, f: F) -> B
            where
                F: FnMut(B, u128) -> B,
            {
                self.inner.rfold(init, f)
            }
        }

        #[cfg(target_pointer_width = "64")]
        impl ExactSizeIterator for $ty {}

        impl FusedIterator for $ty {}
    };
}

iter_api!(Iter<'_>);
iter_api!(IntoIter);

/// Converts the bounds of `range()` / `into_range()`, panicking like `roaring` on invalid ones.
/// Returns `None` for an empty range.
fn iterator_range<R: RangeBounds<u128>>(range: R) -> Option<(u128, u128)> {
    match convert_range_to_inclusive(range) {
        Ok(range) => Some((*range.start(), *range.end())),
        Err(ConvertRangeError::Empty) => None,
        Err(ConvertRangeError::StartGreaterThanEnd) => {
            panic!("range start is greater than range end")
        }
        Err(ConvertRangeError::StartAndEndEqualExcluded) => {
            panic!("range start and end are equal and excluded")
        }
    }
}

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
        Iter::new(self)
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
        let mut iter = self.iter();
        let Some((start, end)) = iterator_range(range) else {
            iter.inner.middle.clear();
            *iter.inner.middle_len.get_mut() = 0;
            return iter;
        };
        if start != 0 {
            iter.advance_to(start);
        }
        if end != u128::MAX {
            iter.advance_back_to(end);
        }
        iter
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
        let Some((start, end)) = iterator_range(range) else {
            return IntoIter::empty();
        };
        // Cut the index down to the keys of the range first (O(log n) splits), so that only
        // those containers are converted.
        let (start_key, _) = split(start);
        let (end_key, _) = split(end);
        let mut map = self.map;
        if start_key > 0 {
            map = map.split_off(&start_key);
        }
        if end_key < MAX_KEY {
            drop(map.split_off(&(end_key + 1)));
        }
        let mut len = 0;
        let entries = map
            .into_iter()
            .inspect(|(_, c)| len += c.len())
            .collect::<Vec<_>>();
        let mut iter = IntoIter::from_entries(entries, len);
        if start != 0 {
            iter.advance_to(start);
        }
        if end != u128::MAX {
            iter.advance_back_to(end);
        }
        iter
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
        let mut iterator = iterator.into_iter();
        let first = match (iterator.next(), self.max()) {
            (None, _) => return Ok(0),
            (Some(first), Some(max)) if first <= max => {
                return Err(NonSortedIntegers { valid_until: 0 });
            }
            (Some(first), _) => first,
        };
        let mut appender = Appender::new(self, first);
        for value in iterator {
            if value <= appender.last {
                return Err(NonSortedIntegers {
                    valid_until: appender.added,
                });
            }
            appender.push(value);
        }
        Ok(appender.added)
    }

    /// Inserts values in any order, batching consecutive values of the same key so that the
    /// B-tree is only searched when the key changes.
    fn extend_unsorted(&mut self, first: u128, rest: impl Iterator<Item = u128>) {
        let RoaringBitmap128 { map, len } = self;
        let (mut current_key, index) = split(first);
        let mut container = map.entry(current_key).or_default();
        *len += container.insert(index) as u64;
        for value in rest {
            let (key, index) = split(value);
            if key != current_key {
                current_key = key;
                container = map.entry(key).or_default();
            }
            *len += container.insert(index) as u64;
        }
    }
}

/// Appends strictly increasing values above the maximum of a bitmap.
///
/// Values go into the last container with `push_unchecked`; new containers are buffered and
/// added to the index in one go when the appender is dropped (a bulk build or a linear
/// `append` for many new keys instead of one B-tree insert per key). Dropping also runs on
/// unwinding, so a panicking source iterator leaves the bitmap consistent.
struct Appender<'a> {
    rb: &'a mut RoaringBitmap128,
    /// Key of `container`.
    key: u128,
    /// The last container, taken out of the index.
    container: Container,
    /// Finished new containers, strictly increasing keys, all below `key`.
    pending: Vec<(u128, Container)>,
    /// Number of appended values (not yet added to `rb.len`).
    added: u64,
    /// The last appended value.
    last: u128,
}

impl<'a> Appender<'a> {
    /// `first` must be greater than the maximum of `rb`.
    fn new(rb: &'a mut RoaringBitmap128, first: u128) -> Appender<'a> {
        debug_assert!(rb.max().is_none_or(|max| first > max));
        let (key, index) = split(first);
        let container = match rb.map.last_entry() {
            Some(entry) if *entry.key() == key => {
                let mut container = entry.remove();
                container.push_unchecked(index);
                container
            }
            _ => Container::with_single(index),
        };
        Appender {
            rb,
            key,
            container,
            pending: Vec::new(),
            added: 1,
            last: first,
        }
    }

    /// `value` must be greater than `self.last`.
    #[inline]
    fn push(&mut self, value: u128) {
        debug_assert!(value > self.last);
        let (key, index) = split(value);
        if key == self.key {
            self.container.push_unchecked(index);
        } else {
            let done = mem::replace(&mut self.container, Container::with_single(index));
            self.pending.push((self.key, done));
            self.key = key;
        }
        self.added += 1;
        self.last = value;
    }
}

impl Drop for Appender<'_> {
    fn drop(&mut self) {
        let mut pending = mem::take(&mut self.pending);
        pending.push((self.key, mem::take(&mut self.container)));
        self.rb.merge_new_entries(pending);
        self.rb.len += self.added;
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
        let len = self.len;
        IntoIter::from_entries(self.map.into_iter().collect(), len)
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
        let mut iterator = iterator.into_iter();
        let Some(first) = iterator.next() else {
            return;
        };
        if self.max().is_some_and(|max| first <= max) {
            return self.extend_unsorted(first, iterator);
        }
        // Sorted fast path: as long as the values increase (duplicates are skipped), they are
        // appended and the new containers bulk-built. The first value out of order switches to
        // the general path.
        let mut out_of_order = None;
        {
            let mut appender = Appender::new(self, first);
            for value in iterator.by_ref() {
                if value > appender.last {
                    appender.push(value);
                } else if value < appender.last {
                    out_of_order = Some(value);
                    break;
                }
            }
        }
        if let Some(value) = out_of_order {
            self.extend_unsorted(value, iterator);
        }
    }
}

impl<'a> Extend<&'a u128> for RoaringBitmap128 {
    fn extend<I: IntoIterator<Item = &'a u128>>(&mut self, iterator: I) {
        self.extend(iterator.into_iter().copied())
    }
}
