//! Binary set operations on the key index.
//!
//! Strategy (see [`super::is_skewed`] and [`super::is_skewed_update`]):
//!
//! * sizes within [`super::SKEW_RATIO`] of each other: a linear merge-join over the two key
//!   ordered maps, `O(n + m)`;
//! * very different sizes: walk the small side and do `O(log n)` lookups / entries in the big
//!   one, `O(m log n)`.
//!
//! Results of `&a op &b` are collected as sorted `(key, container)` vectors and bulk-built.
//! In-place operations update the containers of `self` where they are and only rebuild the index
//! when many keys are added (unions, symmetric differences: new keys are merged in one linear
//! `append`) or most keys are expected to disappear (intersections). The cached cardinality is
//! updated by the delta of every touched container.

use alloc::collections::BTreeMap;
use alloc::collections::btree_map::Entry;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::iter::Peekable;
use core::mem;
use core::ops::ControlFlow;

use super::{is_skewed, is_skewed_update};
use crate::RoaringBitmap128;
use crate::container::Container;

pub(crate) type Map = BTreeMap<u128, Container>;

/// One step of a merge-join.
pub(crate) enum Side<A, B> {
    Left(A),
    Right(B),
    Both(A, B),
}

/// Merge-join of two strictly increasing `(key, item)` sequences.
pub(crate) struct Merge<I: Iterator, J: Iterator> {
    left: Peekable<I>,
    right: Peekable<J>,
}

impl<A, B, I, J> Merge<I, J>
where
    I: Iterator<Item = (u128, A)>,
    J: Iterator<Item = (u128, B)>,
{
    pub(crate) fn new(left: I, right: J) -> Self {
        Merge {
            left: left.peekable(),
            right: right.peekable(),
        }
    }
}

impl<A, B, I, J> Iterator for Merge<I, J>
where
    I: Iterator<Item = (u128, A)>,
    J: Iterator<Item = (u128, B)>,
{
    type Item = (u128, Side<A, B>);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let order = match (self.left.peek(), self.right.peek()) {
            (None, None) => return None,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some((a, _)), Some((b, _))) => a.cmp(b),
        };
        match order {
            Ordering::Less => self.left.next().map(|(k, a)| (k, Side::Left(a))),
            Ordering::Greater => self.right.next().map(|(k, b)| (k, Side::Right(b))),
            Ordering::Equal => {
                let (k, a) = self.left.next()?;
                let (_, b) = self.right.next()?;
                Some((k, Side::Both(a, b)))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (l, lu) = self.left.size_hint();
        let (r, ru) = self.right.size_hint();
        let upper = lu.zip(ru).and_then(|(a, b)| a.checked_add(b));
        (l.max(r), upper)
    }
}

#[inline]
fn entries(map: &Map) -> impl DoubleEndedIterator<Item = (u128, &Container)> {
    map.iter().map(|(&k, c)| (k, c))
}

/// The intersection `[lo, hi]` of the key spans of two maps, if any.
#[inline]
fn common_span(a: &Map, b: &Map) -> Option<(u128, u128)> {
    let lo = (*a.first_key_value()?.0).max(*b.first_key_value()?.0);
    let hi = (*a.last_key_value()?.0).min(*b.last_key_value()?.0);
    (lo <= hi).then_some((lo, hi))
}

/// The key span `[first, last]` of a map, if not empty.
#[inline]
pub(crate) fn span(map: &Map) -> Option<(u128, u128)> {
    Some((*map.first_key_value()?.0, *map.last_key_value()?.0))
}

/// Calls `f` for every key present in both maps, in increasing key order, until it breaks.
pub(crate) fn for_each_common<'a, 'b>(
    a: &'a Map,
    b: &'b Map,
    mut f: impl FnMut(u128, &'a Container, &'b Container) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let Some((lo, hi)) = common_span(a, b) else {
        return ControlFlow::Continue(());
    };
    if is_skewed(a.len(), b.len()) {
        for (&k, ca) in a.range(lo..=hi) {
            if let Some(cb) = b.get(&k) {
                f(k, ca, cb)?;
            }
        }
    } else if is_skewed(b.len(), a.len()) {
        for (&k, cb) in b.range(lo..=hi) {
            if let Some(ca) = a.get(&k) {
                f(k, ca, cb)?;
            }
        }
    } else {
        let mut ia = a.range(lo..=hi);
        let mut ib = b.range(lo..=hi);
        let (mut x, mut y) = (ia.next(), ib.next());
        while let (Some((&ka, ca)), Some((&kb, cb))) = (x, y) {
            match ka.cmp(&kb) {
                Ordering::Less => x = ia.next(),
                Ordering::Greater => y = ib.next(),
                Ordering::Equal => {
                    f(ka, ca, cb)?;
                    x = ia.next();
                    y = ib.next();
                }
            }
        }
    }
    ControlFlow::Continue(())
}

/// Calls `f` for every key of `a` with the container of the same key in `b`, if any, in
/// increasing key order.
fn for_each_left<'a, 'b>(
    a: &'a Map,
    b: &'b Map,
    mut f: impl FnMut(u128, &'a Container, Option<&'b Container>),
) {
    if is_skewed(a.len(), b.len()) {
        for (&k, ca) in a {
            f(k, ca, b.get(&k));
        }
    } else {
        for (k, side) in Merge::new(entries(a), entries(b)) {
            match side {
                Side::Left(ca) => f(k, ca, None),
                Side::Both(ca, cb) => f(k, ca, Some(cb)),
                Side::Right(_) => {}
            }
        }
    }
}

/// Pushes a non empty container to a result under construction.
#[inline]
fn push(entries: &mut Vec<(u128, Container)>, len: &mut u64, key: u128, container: Container) {
    if !container.is_empty() {
        *len += container.len();
        entries.push((key, container));
    }
}

impl RoaringBitmap128 {
    #[inline]
    fn containers(&self) -> usize {
        self.map.len()
    }

    // -----------------------------------------------------------------------------------------
    // Cardinalities and predicates
    // -----------------------------------------------------------------------------------------

    pub(crate) fn intersection_len_impl(&self, other: &RoaringBitmap128) -> u64 {
        let mut sum = 0;
        let _ = for_each_common(&self.map, &other.map, |_, a, b| {
            sum += a.intersection_len(b);
            ControlFlow::Continue(())
        });
        sum
    }

    pub(crate) fn is_disjoint_impl(&self, other: &RoaringBitmap128) -> bool {
        for_each_common(&self.map, &other.map, |_, a, b| {
            if a.is_disjoint(b) {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        })
        .is_continue()
    }

    pub(crate) fn is_subset_impl(&self, other: &RoaringBitmap128) -> bool {
        // Every value and every key of `self` must be in `other`.
        if self.len > other.len || self.containers() > other.containers() {
            return false;
        }
        let Some((lo, hi)) = span(&self.map) else {
            return true;
        };
        let Some((other_lo, other_hi)) = span(&other.map) else {
            return false;
        };
        if lo < other_lo || hi > other_hi {
            return false;
        }
        if is_skewed(self.containers(), other.containers()) {
            return self
                .map
                .iter()
                .all(|(k, c)| other.map.get(k).is_some_and(|o| c.is_subset(o)));
        }
        let mut theirs = other.map.range(lo..=hi);
        'outer: for (&key, container) in &self.map {
            for (&other_key, other_container) in theirs.by_ref() {
                match other_key.cmp(&key) {
                    Ordering::Less => continue,
                    Ordering::Equal if container.is_subset(other_container) => continue 'outer,
                    _ => return false,
                }
            }
            return false;
        }
        true
    }

    // -----------------------------------------------------------------------------------------
    // Union
    // -----------------------------------------------------------------------------------------

    pub(crate) fn union_ref(&self, other: &RoaringBitmap128) -> RoaringBitmap128 {
        let (small, big) = if self.containers() <= other.containers() {
            (self, other)
        } else {
            (other, self)
        };
        if small.map.is_empty() || is_skewed_update(small.containers(), big.containers()) {
            let mut result = big.clone();
            result.union_assign_ref(small);
            return result;
        }
        let mut out = Vec::with_capacity(self.containers() + other.containers());
        let mut len = 0;
        for (k, side) in Merge::new(entries(&self.map), entries(&other.map)) {
            let container = match side {
                Side::Left(a) => a.clone(),
                Side::Right(b) => b.clone(),
                Side::Both(a, b) => a.union(b),
            };
            push(&mut out, &mut len, k, container);
        }
        RoaringBitmap128::from_sorted_entries_with_len(out, len)
    }

    pub(crate) fn union_assign_ref(&mut self, other: &RoaringBitmap128) {
        let Some((lo, hi)) = span(&other.map) else {
            return;
        };
        if self.map.is_empty() {
            self.clone_from(other);
            return;
        }
        if is_skewed_update(self.containers(), other.containers()) {
            // Cheaper to clone the big side and fold the small one into it.
            let small = mem::replace(self, other.clone());
            self.union_assign_owned(small);
            return;
        }
        let RoaringBitmap128 { map, len } = self;
        if is_skewed_update(other.containers(), map.len()) {
            for (&k, b) in &other.map {
                match map.entry(k) {
                    Entry::Vacant(entry) => {
                        *len += b.len();
                        entry.insert(b.clone());
                    }
                    Entry::Occupied(mut entry) => {
                        let a = entry.get_mut();
                        let before = a.len();
                        a.union_with(b);
                        *len += a.len() - before;
                    }
                }
            }
            return;
        }
        // Update the common containers in place, collect the new ones.
        let mut theirs = other.map.iter().peekable();
        let mut new = Vec::new();
        for (&k, a) in map.range_mut(lo..=hi) {
            while let Some((&kb, b)) = theirs.next_if(|(kb, _)| **kb < k) {
                *len += b.len();
                new.push((kb, b.clone()));
            }
            if let Some((_, b)) = theirs.next_if(|(kb, _)| **kb == k) {
                let before = a.len();
                a.union_with(b);
                *len += a.len() - before;
            }
        }
        for (&kb, b) in theirs {
            *len += b.len();
            new.push((kb, b.clone()));
        }
        self.merge_new_entries(new);
    }

    pub(crate) fn union_assign_owned(&mut self, mut other: RoaringBitmap128) {
        if self.containers() < other.containers() {
            mem::swap(self, &mut other);
        }
        let Some((lo, hi)) = span(&other.map) else {
            return;
        };
        let count = other.containers();
        self.union_owned_entries(other.map.into_iter(), count, lo, hi);
    }

    /// Unions `count` owned containers with strictly increasing keys spanning `lo..=hi` into
    /// `self`, reusing their allocations.
    pub(crate) fn union_owned_entries(
        &mut self,
        entries: impl Iterator<Item = (u128, Container)>,
        count: usize,
        lo: u128,
        hi: u128,
    ) {
        let RoaringBitmap128 { map, len } = self;
        if is_skewed_update(count, map.len()) {
            for (k, b) in entries {
                match map.entry(k) {
                    Entry::Vacant(entry) => {
                        *len += b.len();
                        entry.insert(b);
                    }
                    Entry::Occupied(mut entry) => {
                        let a = entry.get_mut();
                        let before = a.len();
                        a.union_with_owned(b);
                        *len += a.len() - before;
                    }
                }
            }
            return;
        }
        // Update the common containers in place, collect the new ones.
        let mut theirs = entries.peekable();
        let mut new = Vec::new();
        for (&k, a) in map.range_mut(lo..=hi) {
            while let Some((kb, b)) = theirs.next_if(|(kb, _)| *kb < k) {
                *len += b.len();
                new.push((kb, b));
            }
            if let Some((_, b)) = theirs.next_if(|(kb, _)| *kb == k) {
                let before = a.len();
                a.union_with_owned(b);
                *len += a.len() - before;
            }
        }
        for (kb, b) in theirs {
            *len += b.len();
            new.push((kb, b));
        }
        self.merge_new_entries(new);
    }

    // -----------------------------------------------------------------------------------------
    // Intersection
    // -----------------------------------------------------------------------------------------

    pub(crate) fn intersection_ref(&self, other: &RoaringBitmap128) -> RoaringBitmap128 {
        let mut out = Vec::new();
        let mut len = 0;
        let _ = for_each_common(&self.map, &other.map, |k, a, b| {
            push(&mut out, &mut len, k, a.intersection(b));
            ControlFlow::Continue(())
        });
        RoaringBitmap128::from_sorted_entries_with_len(out, len)
    }

    pub(crate) fn intersect_assign_ref(&mut self, other: &RoaringBitmap128) {
        if self.map.is_empty() {
            return;
        }
        if other.map.is_empty() {
            self.clear();
            return;
        }
        if is_skewed(self.containers(), other.containers()) {
            // Few containers here: update them in place with lookups in `other`.
            let mut removed = 0;
            self.map.retain(|k, a| match other.map.get(k) {
                None => {
                    removed += a.len();
                    false
                }
                Some(b) => {
                    let before = a.len();
                    a.intersect_with(b);
                    removed += before - a.len();
                    !a.is_empty()
                }
            });
            self.len -= removed;
            return;
        }
        // The result is at most as large as the smaller side and usually much smaller than
        // `self`: bulk-build the survivors instead of removing most entries one by one.
        let old = mem::take(&mut self.map);
        let mut out = Vec::new();
        let mut len = 0;
        if is_skewed(other.containers(), old.len()) {
            for (&k, b) in &other.map {
                if let Some(a) = old.get(&k) {
                    push(&mut out, &mut len, k, a.intersection(b));
                }
            }
            drop(old);
        } else {
            for (k, side) in Merge::new(old.into_iter(), entries(&other.map)) {
                if let Side::Both(mut a, b) = side {
                    a.intersect_with(b);
                    push(&mut out, &mut len, k, a);
                }
            }
        }
        *self = RoaringBitmap128::from_sorted_entries_with_len(out, len);
    }

    pub(crate) fn intersect_assign_owned(&mut self, mut other: RoaringBitmap128) {
        if other.containers() < self.containers() {
            mem::swap(self, &mut other);
        }
        if self.map.is_empty() || is_skewed(self.containers(), other.containers()) {
            return self.intersect_assign_ref(&other);
        }
        let old = mem::take(&mut self.map);
        let mut out = Vec::new();
        let mut len = 0;
        for (k, side) in Merge::new(old.into_iter(), other.map.into_iter()) {
            if let Side::Both(mut a, b) = side {
                a.intersect_with_owned(b);
                push(&mut out, &mut len, k, a);
            }
        }
        *self = RoaringBitmap128::from_sorted_entries_with_len(out, len);
    }

    // -----------------------------------------------------------------------------------------
    // Difference
    // -----------------------------------------------------------------------------------------

    pub(crate) fn difference_ref(&self, other: &RoaringBitmap128) -> RoaringBitmap128 {
        if self.map.is_empty() {
            return RoaringBitmap128::new();
        }
        if other.map.is_empty() || is_skewed(other.containers(), self.containers()) {
            let mut result = self.clone();
            result.difference_assign_ref(other);
            return result;
        }
        let mut out = Vec::with_capacity(self.containers());
        let mut len = 0;
        for_each_left(&self.map, &other.map, |k, a, b| {
            let container = match b {
                None => a.clone(),
                Some(b) => a.difference(b),
            };
            push(&mut out, &mut len, k, container);
        });
        RoaringBitmap128::from_sorted_entries_with_len(out, len)
    }

    pub(crate) fn difference_assign_ref(&mut self, other: &RoaringBitmap128) {
        let Some((lo, hi)) = common_span(&self.map, &other.map) else {
            return;
        };
        let RoaringBitmap128 { map, len } = self;
        if is_skewed(other.containers(), map.len()) {
            for (&k, b) in other.map.range(lo..=hi) {
                if let Entry::Occupied(mut entry) = map.entry(k) {
                    let a = entry.get_mut();
                    let before = a.len();
                    a.difference_with(b);
                    *len -= before - a.len();
                    if a.is_empty() {
                        entry.remove();
                    }
                }
            }
            return;
        }
        let lookups = is_skewed(map.len(), other.containers());
        let mut theirs = other.map.range(lo..=hi).peekable();
        let mut removed = 0;
        // One in-place pass over the common key span; emptied containers are dropped on the way.
        map.extract_if(lo..=hi, |k, a| {
            let b = if lookups {
                other.map.get(k)
            } else {
                while theirs.next_if(|(kb, _)| *kb < k).is_some() {}
                theirs.next_if(|(kb, _)| *kb == k).map(|(_, b)| b)
            };
            let Some(b) = b else {
                return false;
            };
            let before = a.len();
            a.difference_with(b);
            removed += before - a.len();
            a.is_empty()
        })
        .for_each(drop);
        *len -= removed;
    }

    // -----------------------------------------------------------------------------------------
    // Symmetric difference
    // -----------------------------------------------------------------------------------------

    pub(crate) fn symmetric_difference_ref(&self, other: &RoaringBitmap128) -> RoaringBitmap128 {
        let (small, big) = if self.containers() <= other.containers() {
            (self, other)
        } else {
            (other, self)
        };
        if small.map.is_empty() || is_skewed_update(small.containers(), big.containers()) {
            let mut result = big.clone();
            result.symmetric_difference_assign_ref(small);
            return result;
        }
        let mut out = Vec::with_capacity(self.containers() + other.containers());
        let mut len = 0;
        for (k, side) in Merge::new(entries(&self.map), entries(&other.map)) {
            let container = match side {
                Side::Left(a) => a.clone(),
                Side::Right(b) => b.clone(),
                Side::Both(a, b) => a.symmetric_difference(b),
            };
            push(&mut out, &mut len, k, container);
        }
        RoaringBitmap128::from_sorted_entries_with_len(out, len)
    }

    pub(crate) fn symmetric_difference_assign_ref(&mut self, other: &RoaringBitmap128) {
        let Some((lo, hi)) = span(&other.map) else {
            return;
        };
        if self.map.is_empty() {
            self.clone_from(other);
            return;
        }
        if is_skewed_update(self.containers(), other.containers()) {
            let small = mem::replace(self, other.clone());
            self.symmetric_difference_assign_owned(small);
            return;
        }
        let RoaringBitmap128 { map, len } = self;
        if is_skewed_update(other.containers(), map.len()) {
            for (&k, b) in &other.map {
                match map.entry(k) {
                    Entry::Vacant(entry) => {
                        *len += b.len();
                        entry.insert(b.clone());
                    }
                    Entry::Occupied(mut entry) => {
                        let a = entry.get_mut();
                        *len -= a.len();
                        a.symmetric_difference_with(b);
                        *len += a.len();
                        if a.is_empty() {
                            entry.remove();
                        }
                    }
                }
            }
            return;
        }
        let mut theirs = other.map.iter().peekable();
        let mut new = Vec::new();
        let mut added = 0;
        let mut removed = 0;
        map.extract_if(lo..=hi, |&k, a| {
            while let Some((&kb, b)) = theirs.next_if(|(kb, _)| **kb < k) {
                added += b.len();
                new.push((kb, b.clone()));
            }
            let Some((_, b)) = theirs.next_if(|(kb, _)| **kb == k) else {
                return false;
            };
            removed += a.len();
            a.symmetric_difference_with(b);
            added += a.len();
            a.is_empty()
        })
        .for_each(drop);
        for (&kb, b) in theirs {
            added += b.len();
            new.push((kb, b.clone()));
        }
        *len = *len + added - removed;
        self.merge_new_entries(new);
    }

    pub(crate) fn symmetric_difference_assign_owned(&mut self, mut other: RoaringBitmap128) {
        if self.containers() < other.containers() {
            mem::swap(self, &mut other);
        }
        let Some((lo, hi)) = span(&other.map) else {
            return;
        };
        let RoaringBitmap128 { map, len } = self;
        if is_skewed_update(other.containers(), map.len()) {
            for (k, b) in other.map {
                match map.entry(k) {
                    Entry::Vacant(entry) => {
                        *len += b.len();
                        entry.insert(b);
                    }
                    Entry::Occupied(mut entry) => {
                        let a = entry.get_mut();
                        *len -= a.len();
                        a.symmetric_difference_with_owned(b);
                        *len += a.len();
                        if a.is_empty() {
                            entry.remove();
                        }
                    }
                }
            }
            return;
        }
        let mut theirs = other.map.into_iter().peekable();
        let mut new = Vec::new();
        let mut added = 0;
        let mut removed = 0;
        map.extract_if(lo..=hi, |&k, a| {
            while let Some((kb, b)) = theirs.next_if(|(kb, _)| *kb < k) {
                added += b.len();
                new.push((kb, b));
            }
            let Some((_, b)) = theirs.next_if(|(kb, _)| *kb == k) else {
                return false;
            };
            removed += a.len();
            a.symmetric_difference_with_owned(b);
            added += a.len();
            a.is_empty()
        })
        .for_each(drop);
        for (kb, b) in theirs {
            added += b.len();
            new.push((kb, b));
        }
        *len = *len + added - removed;
        self.merge_new_entries(new);
    }
}
